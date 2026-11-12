# Setting up Google Drive in your own build

Copies of Vardo you build yourself can open files on your computer straight
away. Google Drive needs one extra step first, because Google requires every
app that signs in to identify itself with its own credentials. This guide
creates those credentials in a free Google Cloud project. It takes about ten
minutes and costs nothing.

You only do this once. The credentials go into the program when you build it.

## 1. Create a project

1. Go to the [Google Cloud console](https://console.cloud.google.com) and sign
   in with your Google account.
2. Open the project menu at the top of the page and choose **New project**.
   Call it something you will recognise, such as `vardo`, and create it.
3. Make sure the new project is selected in the project menu before you
   continue.

## 2. Turn on the two Google services Vardo uses

Open each link with your new project selected and press **Enable**:

- [Google Drive API](https://console.cloud.google.com/apis/library/drive.googleapis.com),
  which lets Vardo read and save your vault.
- [Google Picker API](https://console.cloud.google.com/apis/library/picker.googleapis.com),
  which shows Google's own file chooser when you pick an existing vault.

## 3. Describe the app to Google

Google shows this information on the screen where you agree to let Vardo in.

1. Open **[Google Auth Platform](https://console.cloud.google.com/auth/overview)**
   and press **Get started**.
2. **App information:** an app name such as `Vardo`, and your email address as
   the support email.
3. **Audience:** choose **External**. "Internal" is only available to Google
   Workspace organisations.
4. **Contact information:** your email address again.
5. Agree to the policy and press **Create**.

### Stop Google signing you out every week

New apps start in **Testing** mode, and Google expires sign-ins for apps in
Testing mode after seven days. You would have to sign in to Drive again every
week.

To avoid that, open **Audience** and press **Publish app**. Because Vardo only
asks for permission to see the files you give it, Google treats it as a
low-risk app and publishes it without a review.

If you would rather stay in Testing mode, add your own email address under
**Test users** on the same page, and expect to sign in again weekly.

## 4. Create the credentials

1. Open **[Clients](https://console.cloud.google.com/auth/clients)** and press
   **Create client**.
2. For **Application type**, choose **Desktop app**. Name it anything.
3. Press **Create**. Google shows a **Client ID** and a **Client secret**. Keep
   this page open for the next step.

You do not need to set up a redirect address. Desktop apps sign in through a
temporary address on your own computer, and Google allows that automatically.

## 5. Build Vardo with them

Put the two values into your environment, then build as usual.

On Linux and macOS:

```sh
export VARDO_CLIENT_ID="paste the client ID here"
export VARDO_CLIENT_SECRET="paste the client secret here"
cd vardo/app
npm run app:release
```

On Windows, in PowerShell:

```powershell
$env:VARDO_CLIENT_ID = "paste the client ID here"
$env:VARDO_CLIENT_SECRET = "paste the client secret here"
cd vardo\app
npm run app:release
```

To avoid pasting them every time, keep the two `export` lines in a file only
you can read, and load it before building:

```sh
mkdir -p ~/.config/vardo
nano ~/.config/vardo/oauth-client.env    # the two export lines go here
chmod 600 ~/.config/vardo/oauth-client.env

set -a; . ~/.config/vardo/oauth-client.env; set +a
npm run app:release
```

## 6. Check that it worked

Start Vardo and choose **Google Drive**. You should see a **Sign in** button.
If you see "Unavailable: this build has no Google client id" instead, the
variables were not set in the terminal you built from. Set them and build
again.

## Questions

**Is the "client secret" really secret?**
Not for a desktop app, and Google says so. Anything built into a program on
your computer can be read out of it. Google relies on other protections for
desktop apps: the sign-in happens in your own browser, and every grant is tied
to the account that approved it. Still, do not publish it, because anyone who
has it can make requests that count against your project's free quota.

**Will this cost me anything?**
No. Google's free allowance is far beyond what one person's password vault
uses. You do not need to add a payment method.

**Can I use the same credentials on several computers?**
Yes. Build Vardo with the same two values on each one.

**What can these credentials reach?**
Only files Vardo created or that you picked for it, in whichever Google account
signs in. They cannot see anything else in your Drive.
